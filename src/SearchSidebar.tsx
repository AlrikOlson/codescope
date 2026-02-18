import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { FileIcon } from './icons';
import { HighlightedText, EMPTY_SEARCH, buildFlatResults } from './search-utils';
import type { Manifest, SearchResponse, GrepResponse } from './types';
import './styles/sidebar.css';

interface Props {
  manifest: Manifest;
  selected: Set<string>;
  onNavigateModule: (id: string) => void;
  onToggleFile: (path: string) => void;
  onPreview?: (path: string) => void;
  onSmartSelect: (paths: string[], scores: Map<string, number>, query: string) => void;
  onGlobalSearchPaths: (paths: Map<string, number>, label: string) => void;
  autoFocus?: boolean;
}

export default function SearchSidebar({
  manifest, selected, onNavigateModule, onToggleFile, onPreview, onSmartSelect, onGlobalSearchPaths, autoFocus,
}: Props) {
  const [query, setQuery] = useState('');
  const [activeIdx, setActiveIdx] = useState(0);
  const [results, setResults] = useState<SearchResponse>(EMPTY_SEARCH);
  const [grepResults, setGrepResults] = useState<GrepResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoFocus) inputRef.current?.focus();
  }, [autoFocus]);

  // Unified search
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setResults(EMPTY_SEARCH);
      setGrepResults(null);
      return;
    }

    setLoading(true);
    const controller = new AbortController();

    const timer = setTimeout(() => {
      const fileSearch = fetch(
        `/api/search?q=${encodeURIComponent(q)}&fileLimit=50&moduleLimit=8`,
        { signal: controller.signal }
      ).then(r => r.json() as Promise<SearchResponse>).catch(() => EMPTY_SEARCH);

      const grepSearch = q.length >= 2
        ? fetch(
            `/api/grep?q=${encodeURIComponent(q)}&limit=50&maxPerFile=3`,
            { signal: controller.signal }
          ).then(r => r.json() as Promise<GrepResponse>).catch(() => null)
        : Promise.resolve(null);

      Promise.all([fileSearch, grepSearch]).then(([fileData, grepData]) => {
        setResults(fileData);
        setGrepResults(grepData);
        setLoading(false);
      });
    }, 150);

    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [query]);

  const flatResults = useMemo(() => buildFlatResults(results, grepResults), [results, grepResults]);

  const selectableIndices = useMemo(
    () => flatResults
      .map((r, i) => (r.type === 'module-header' || r.type === 'file-header' || r.type === 'content-header') ? -1 : i)
      .filter(i => i >= 0),
    [flatResults],
  );

  const virtualizer = useVirtualizer({
    count: flatResults.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => {
      const item = flatResults[i];
      if (!item) return 32;
      if (item.type === 'module-header' || item.type === 'file-header' || item.type === 'content-header') return 32;
      if (item.type === 'module') return 36;
      if (item.type === 'grep-file') return 34;
      if (item.type === 'grep-match') return 28;
      return 48; // file (two lines: name + path)
    },
    overscan: 15,
  });

  useEffect(() => {
    setActiveIdx(0);
  }, [query]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActiveIdx(prev => {
        const curSel = selectableIndices.indexOf(prev);
        const next = curSel < selectableIndices.length - 1 ? selectableIndices[curSel + 1] : selectableIndices[0];
        virtualizer.scrollToIndex(next ?? 0);
        return next ?? 0;
      });
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActiveIdx(prev => {
        const curSel = selectableIndices.indexOf(prev);
        const next = curSel > 0 ? selectableIndices[curSel - 1] : selectableIndices[selectableIndices.length - 1];
        virtualizer.scrollToIndex(next ?? 0);
        return next ?? 0;
      });
    }
    if (e.key === 'Enter') {
      const item = flatResults[activeIdx];
      if (!item) return;
      if (item.type === 'module') {
        onNavigateModule(item.data.id);
      } else if (item.type === 'file') {
        onToggleFile(item.data.path);
      } else if (item.type === 'grep-file') {
        onToggleFile(item.data.path);
      } else if (item.type === 'grep-match') {
        if (onPreview) onPreview(item.filePath);
        else onToggleFile(item.filePath);
      }
    }
    if (e.key === 'Escape') {
      if (query) {
        setQuery('');
      } else {
        inputRef.current?.blur();
      }
    }
  }, [activeIdx, flatResults, selectableIndices, onNavigateModule, onToggleFile, onPreview, virtualizer, query]);

  const hasQuery = query.trim().length > 0;
  const totalFileMatches = results.files.length;
  const totalModuleMatches = results.modules.length;
  const totalGrepMatches = grepResults?.totalMatches ?? 0;
  const totalGrepFiles = grepResults?.results.length ?? 0;

  // Compute unique file count for smart select
  const uniqueFileCount = useMemo(() => {
    if (!hasQuery) return 0;
    const allPaths = new Set<string>();
    for (const f of results.files) allPaths.add(f.path);
    if (grepResults) for (const g of grepResults.results) allPaths.add(g.path);
    return Math.min(allPaths.size, 50);
  }, [hasQuery, results.files, grepResults]);

  const handleSmartSelect = useCallback(() => {
    const scoreMap = new Map<string, number>();
    for (const f of results.files) scoreMap.set(f.path, Math.max(scoreMap.get(f.path) ?? 0, f.score));
    if (grepResults) {
      for (const g of grepResults.results) scoreMap.set(g.path, Math.max(scoreMap.get(g.path) ?? 0, g.score));
    }
    const ranked = [...scoreMap.entries()].sort((a, b) => b[1] - a[1]).slice(0, 50);
    onSmartSelect(ranked.map(([p]) => p), new Map(ranked), query.trim());
  }, [results.files, grepResults, onSmartSelect, query]);

  const hasSomeResults = totalFileMatches > 0 || totalModuleMatches > 0 || totalGrepFiles > 0;

  return (
    <div className="sidebar search-sidebar">
      {/* Search input */}
      <div className="search-input-row" onKeyDown={handleKeyDown}>
        <div className="search-input-field">
          <svg className="search-input-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>
          <input
            ref={inputRef}
            type="text"
            placeholder="Search files, modules, code..."
            value={query}
            onChange={e => setQuery(e.target.value)}
          />
          {loading && <div className="search-input-spinner" />}
          {hasQuery && !loading && (
            <button className="search-input-clear" onClick={() => { setQuery(''); inputRef.current?.focus(); }}>
              &times;
            </button>
          )}
        </div>
      </div>

      {/* Summary bar — always visible when there are results */}
      {hasQuery && hasSomeResults && (
        <div className="search-summary-bar">
          {totalModuleMatches > 0 && (
            <span className="search-summary-chip">
              <span className="search-summary-count">{totalModuleMatches}</span> modules
            </span>
          )}
          {totalFileMatches > 0 && (
            <span className="search-summary-chip">
              <span className="search-summary-count">{totalFileMatches}</span> files
            </span>
          )}
          {totalGrepMatches > 0 && (
            <span className="search-summary-chip">
              <span className="search-summary-count">{totalGrepMatches}</span> matches
            </span>
          )}
          <span className="search-summary-spacer" />
          {uniqueFileCount > 0 && (
            <button className="search-summary-select" onClick={handleSmartSelect} title="Add top results to context">
              + {uniqueFileCount}
            </button>
          )}
        </div>
      )}

      {/* Results */}
      <div ref={scrollRef} className="search-sidebar-results">
        {!hasQuery && (
          <div className="search-empty-state">
            <div className="search-empty-icon">
              <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
              </svg>
            </div>
            <div className="search-empty-text">Search across files, modules, and source code</div>
            <div className="search-empty-shortcuts">
              <kbd>Enter</kbd> select &middot; <kbd>&uarr;&darr;</kbd> navigate &middot; <kbd>Esc</kbd> clear
            </div>
          </div>
        )}

        {hasQuery && !loading && flatResults.length === 0 && (
          <div className="search-empty-state">
            <div className="search-empty-text">No results for &ldquo;{query}&rdquo;</div>
          </div>
        )}

        {hasQuery && flatResults.length > 0 && (
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualizer.getVirtualItems().map(vi => {
              const item = flatResults[vi.index];
              const isActive = vi.index === activeIdx;
              const posStyle = { position: 'absolute' as const, top: vi.start, height: vi.size, width: '100%' };

              // Section headers
              if (item.type === 'module-header') {
                return (
                  <div key="mh" className="search-section-label" style={posStyle}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor" opacity="0.5">
                      <path d="M10 4H4a2 2 0 00-2 2v12a2 2 0 002 2h16a2 2 0 002-2V8a2 2 0 00-2-2h-8l-2-2z"/>
                    </svg>
                    Modules
                    <span className="search-section-count">{totalModuleMatches}</span>
                  </div>
                );
              }
              if (item.type === 'file-header') {
                return (
                  <div key="fh" className="search-section-label" style={posStyle}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" opacity="0.5">
                      <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline points="14 2 14 8 20 8"/>
                    </svg>
                    Files
                    <span className="search-section-count">{totalFileMatches}</span>
                  </div>
                );
              }
              if (item.type === 'content-header') {
                return (
                  <div key="ch" className="search-section-label" style={posStyle}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" opacity="0.5">
                      <polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>
                    </svg>
                    Code
                    <span className="search-section-count">{totalGrepMatches} in {totalGrepFiles} files</span>
                  </div>
                );
              }

              // Module result
              if (item.type === 'module') {
                const m = item.data;
                return (
                  <div
                    key={`m-${m.id}`}
                    className={`search-result${isActive ? ' active' : ''}`}
                    style={posStyle}
                    onClick={() => onNavigateModule(m.id)}
                    onMouseEnter={() => setActiveIdx(vi.index)}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" className="search-result-icon">
                      <path d="M10 4H4a2 2 0 00-2 2v12a2 2 0 002 2h16a2 2 0 002-2V8a2 2 0 00-2-2h-8l-2-2z"/>
                    </svg>
                    <span className="search-result-text">
                      <HighlightedText text={m.id} indices={m.matchedIndices} />
                    </span>
                    <span className="search-result-badge">{m.fileCount}</span>
                  </div>
                );
              }

              // File result
              if (item.type === 'file') {
                const f = item.data;
                const isSelected = selected.has(f.path);
                return (
                  <div
                    key={`f-${f.path}`}
                    className={`search-result search-result-file${isActive ? ' active' : ''}${isSelected ? ' selected' : ''}`}
                    style={posStyle}
                    onClick={() => onToggleFile(f.path)}
                    onMouseEnter={() => setActiveIdx(vi.index)}
                  >
                    <FileIcon ext={f.ext} size={14} />
                    <div className="search-result-file-info">
                      <span className="search-result-filename">
                        <HighlightedText text={f.filename} indices={f.filenameIndices} />
                      </span>
                      <span className="search-result-path">{f.dir}</span>
                    </div>
                    {isSelected && <span className="search-check" aria-label="Selected">&#10003;</span>}
                  </div>
                );
              }

              // Grep file header
              if (item.type === 'grep-file') {
                const isSelected = selected.has(item.data.path);
                return (
                  <div
                    key={`gf-${item.data.path}`}
                    className={`search-result search-result-grep-file${isActive ? ' active' : ''}${isSelected ? ' selected' : ''}`}
                    style={posStyle}
                    onClick={() => onToggleFile(item.data.path)}
                    onMouseEnter={() => setActiveIdx(vi.index)}
                  >
                    <FileIcon ext={item.data.path.slice(item.data.path.lastIndexOf('.'))} size={13} />
                    <span className="search-result-text">{item.data.path}</span>
                    <span className="search-result-badge">{item.data.matches.length}</span>
                    {isSelected && <span className="search-check" aria-label="Selected">&#10003;</span>}
                  </div>
                );
              }

              // Grep match line
              return (
                <div
                  key={`gm-${item.filePath}-${item.lineNum}`}
                  className={`search-result search-result-grep-match${isActive ? ' active' : ''}`}
                  style={posStyle}
                  onClick={() => {
                    if (onPreview) onPreview(item.filePath);
                    else onToggleFile(item.filePath);
                  }}
                  onMouseEnter={() => setActiveIdx(vi.index)}
                >
                  <span className="grep-linenum">{item.lineNum}</span>
                  <span className="grep-line-text">{item.line}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
