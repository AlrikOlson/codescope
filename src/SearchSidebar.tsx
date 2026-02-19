import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { FileIcon } from './icons';
import { HighlightedText, EMPTY_FIND } from './search-utils';
import { getExtColor } from './colors';
import type { FindResponse, FindResult } from './types';
import './styles/sidebar.css';

interface Props {
  selected: Set<string>;
  onToggleFile: (path: string) => void;
  onPreview?: (path: string) => void;
  onSearchResults: (paths: Map<string, number>, query: string) => void;
  autoFocus?: boolean;
}

export default function SearchSidebar({
  selected, onToggleFile, onPreview, onSearchResults, autoFocus,
}: Props) {
  const [query, setQuery] = useState('');
  const [activeIdx, setActiveIdx] = useState(0);
  const [findResults, setFindResults] = useState<FindResponse>(EMPTY_FIND);
  const [loading, setLoading] = useState(false);
  const [extFilter, setExtFilter] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoFocus) inputRef.current?.focus();
  }, [autoFocus]);

  // Unified find search
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

      fetch(`/api/find?${params}`, { signal: controller.signal })
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
  }, [query, extFilter]);

  // Push search results to parent for FileList display
  useEffect(() => {
    const q = query.trim();
    if (!q || findResults.results.length === 0) return;
    const paths = new Map<string, number>();
    for (const r of findResults.results) {
      paths.set(r.path, r.combinedScore);
    }
    onSearchResults(paths, q);
  }, [findResults, query, onSearchResults]);

  const results = findResults.results;

  const virtualizer = useVirtualizer({
    count: results.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => {
      const item = results[i];
      if (!item) return 52;
      // Cards with content match snippet are taller
      return item.topMatch ? 72 : 52;
    },
    overscan: 15,
  });

  useEffect(() => {
    setActiveIdx(0);
  }, [query, extFilter]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActiveIdx(prev => {
        const next = prev < results.length - 1 ? prev + 1 : 0;
        virtualizer.scrollToIndex(next);
        return next;
      });
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActiveIdx(prev => {
        const next = prev > 0 ? prev - 1 : results.length - 1;
        virtualizer.scrollToIndex(next);
        return next;
      });
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const item = results[activeIdx];
      if (!item) return;
      if (e.ctrlKey || e.metaKey) {
        if (onPreview) onPreview(item.path);
      } else {
        onToggleFile(item.path);
      }
    }
    if (e.key === 'Escape') {
      if (extFilter) {
        setExtFilter(null);
      } else if (query) {
        setQuery('');
      } else {
        inputRef.current?.blur();
      }
    }
  }, [activeIdx, results, onToggleFile, onPreview, virtualizer, query, extFilter]);

  const hasQuery = query.trim().length > 0;

  // Top extension chips from facets
  const topExts = useMemo(() => {
    return Object.entries(findResults.extCounts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 6);
  }, [findResults.extCounts]);

  const totalResults = results.length;

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
            placeholder="Search files and code..."
            value={query}
            onChange={e => setQuery(e.target.value)}
          />
          {loading && <div className="search-input-spinner" />}
          {hasQuery && !loading && (
            <button className="search-input-clear" onClick={() => { setQuery(''); setExtFilter(null); inputRef.current?.focus(); }}>
              &times;
            </button>
          )}
        </div>
      </div>

      {/* Filter bar — extension chips */}
      {hasQuery && topExts.length > 0 && (
        <div className="search-filter-bar">
          {extFilter && (
            <button
              className="search-filter-chip active clear"
              onClick={() => setExtFilter(null)}
              title="Clear filter"
            >
              &times;
            </button>
          )}
          {topExts.map(([ext, count]) => (
            <button
              key={ext}
              className={`search-filter-chip${extFilter === ext ? ' active' : ''}`}
              style={{ '--chip-color': getExtColor(ext) } as React.CSSProperties}
              onClick={() => setExtFilter(prev => prev === ext ? null : ext)}
            >
              .{ext}<span className="search-filter-count">{count}</span>
            </button>
          ))}
          <span className="search-filter-total">{totalResults} results</span>
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
            <div className="search-empty-text">Search across files and source code</div>
            <div className="search-empty-shortcuts">
              <kbd>Enter</kbd> select &middot; <kbd>Ctrl+Enter</kbd> preview &middot; <kbd>&uarr;&darr;</kbd> navigate &middot; <kbd>Esc</kbd> clear
            </div>
          </div>
        )}

        {hasQuery && !loading && results.length === 0 && (
          <div className="search-empty-state">
            <div className="search-empty-text">No results for &ldquo;{query}&rdquo;</div>
            {extFilter && (
              <button className="search-empty-clear-filter" onClick={() => setExtFilter(null)}>
                Clear .{extFilter} filter
              </button>
            )}
          </div>
        )}

        {hasQuery && results.length > 0 && (
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualizer.getVirtualItems().map(vi => {
              const item = results[vi.index];
              const isActive = vi.index === activeIdx;
              const isSelected = selected.has(item.path);
              const borderColor = getExtColor(item.ext);

              return (
                <div
                  key={item.path}
                  className={`search-result-card${isActive ? ' active' : ''}${isSelected ? ' selected' : ''}`}
                  style={{
                    position: 'absolute',
                    top: vi.start,
                    height: vi.size,
                    width: '100%',
                    '--card-accent': borderColor,
                  } as React.CSSProperties}
                  onClick={() => onToggleFile(item.path)}
                  onMouseEnter={() => setActiveIdx(vi.index)}
                >
                  <div className="search-card-main">
                    <FileIcon ext={item.ext} size={14} />
                    <div className="search-card-info">
                      <div className="search-card-top">
                        <span className="search-card-filename">
                          <HighlightedText text={item.filename} indices={item.filenameIndices} />
                        </span>
                        <MatchTypeBadge type={item.matchType} count={item.grepCount} />
                      </div>
                      <span className="search-card-path">{item.dir}/</span>
                    </div>
                    {isSelected && <span className="search-check" aria-label="Selected">&#10003;</span>}
                    <button
                      className="search-card-preview"
                      onClick={e => { e.stopPropagation(); if (onPreview) onPreview(item.path); }}
                      title="Preview"
                    >
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>
                      </svg>
                    </button>
                  </div>
                  {item.topMatch && (
                    <div className="search-card-snippet">
                      {item.topMatchLine && <span className="snippet-linenum">{item.topMatchLine}</span>}
                      <span className="snippet-text">{item.topMatch.trim()}</span>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

function MatchTypeBadge({ type, count }: { type: FindResult['matchType']; count: number }) {
  const label = type === 'both' ? `name+${count}` : type === 'content' ? `${count} matches` : 'name';
  const cls = `match-type-badge match-type-${type}`;
  return <span className={cls}>{label}</span>;
}
