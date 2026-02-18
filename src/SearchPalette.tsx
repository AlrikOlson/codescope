import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { FileIcon } from './icons';
import type { Manifest, SearchResponse, FileSearchResult, ModuleSearchResult, GrepResponse, GrepFileResult } from './types';
import './styles/palette.css';

interface Props {
  manifest: Manifest;
  selected: Set<string>;
  onClose: () => void;
  onNavigateModule: (id: string) => void;
  onToggleFile: (path: string) => void;
  onGlobalSearchPaths: (paths: Map<string, number>, label: string) => void;
  onSmartSelect: (paths: string[], scores: Map<string, number>, query: string) => void;
  onPreview?: (path: string) => void;
}

// Removed suggested searches — palette starts clean

function HighlightedText({ text, indices }: { text: string; indices: number[] }) {
  if (indices.length === 0) return <>{text}</>;
  const set = new Set(indices);
  const parts: JSX.Element[] = [];
  let run = '';
  let inMatch = false;

  for (let i = 0; i < text.length; i++) {
    const isMatch = set.has(i);
    if (isMatch !== inMatch) {
      if (run) {
        parts.push(inMatch ? <mark key={i}>{run}</mark> : <span key={i}>{run}</span>);
      }
      run = '';
      inMatch = isMatch;
    }
    run += text[i];
  }
  if (run) {
    parts.push(inMatch ? <mark key="end">{run}</mark> : <span key="end">{run}</span>);
  }
  return <>{parts}</>;
}

type UnifiedResult =
  | { type: 'module-header' }
  | { type: 'file-header' }
  | { type: 'content-header' }
  | { type: 'module'; data: ModuleSearchResult }
  | { type: 'file'; data: FileSearchResult }
  | { type: 'grep-file'; data: GrepFileResult }
  | { type: 'grep-match'; line: string; lineNum: number; filePath: string };

const EMPTY_SEARCH: SearchResponse = { files: [], modules: [], queryTime: 0, totalFiles: 0, totalModules: 0 };

export default function SearchPalette({ manifest, selected, onClose, onNavigateModule, onToggleFile, onGlobalSearchPaths, onSmartSelect, onPreview }: Props) {
  const [query, setQuery] = useState('');
  const [activeIdx, setActiveIdx] = useState(0);
  const [results, setResults] = useState<SearchResponse>(EMPTY_SEARCH);
  const [grepResults, setGrepResults] = useState<GrepResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Unified search — fire filename + content search in parallel
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

      // Only grep if query is 2+ chars (server requires it)
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

  // Build unified flat list: modules -> files -> content matches
  const flatResults = useMemo<UnifiedResult[]>(() => {
    const items: UnifiedResult[] = [];

    if (results.modules.length > 0) {
      items.push({ type: 'module-header' });
      for (const m of results.modules) items.push({ type: 'module', data: m });
    }

    if (results.files.length > 0) {
      items.push({ type: 'file-header' });
      for (const f of results.files) items.push({ type: 'file', data: f });
    }

    if (grepResults && grepResults.results.length > 0) {
      // Deduplicate: skip grep files already shown in filename results
      const fileResultPaths = new Set(results.files.map(f => f.path));
      const filteredGrep = grepResults.results.filter(g => !fileResultPaths.has(g.path));

      if (filteredGrep.length > 0 || grepResults.results.length > 0) {
        items.push({ type: 'content-header' });
        const grepToShow = grepResults.results;
        for (const file of grepToShow) {
          items.push({ type: 'grep-file', data: file });
          for (const match of file.matches) {
            items.push({ type: 'grep-match', line: match.line, lineNum: match.lineNum, filePath: file.path });
          }
        }
      }
    }

    return items;
  }, [results, grepResults]);

  // Selectable items only (skip headers)
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
      if (item.type === 'module') return 40;
      if (item.type === 'grep-file') return 36;
      if (item.type === 'grep-match') return 28;
      return 52; // file
    },
    overscan: 15,
  });

  useEffect(() => {
    setActiveIdx(0);
  }, [query]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      onClose();
      return;
    }
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
        if (!e.shiftKey) onClose();
      } else if (item.type === 'file') {
        onToggleFile(item.data.path);
        if (!e.shiftKey) onClose();
      } else if (item.type === 'grep-file') {
        onToggleFile(item.data.path);
        if (!e.shiftKey) onClose();
      } else if (item.type === 'grep-match') {
        if (onPreview) {
          onPreview(item.filePath);
        } else {
          onToggleFile(item.filePath);
        }
        if (!e.shiftKey) onClose();
      }
    }
  }, [activeIdx, flatResults, selectableIndices, onClose, onNavigateModule, onToggleFile, onPreview, virtualizer]);

  const hasQuery = query.trim().length > 0;
  const totalFileMatches = results.files.length;
  const totalModuleMatches = results.modules.length;
  const totalGrepMatches = grepResults?.totalMatches ?? 0;
  const totalGrepFiles = grepResults?.results.length ?? 0;

  return (
    <div className="palette-overlay" onClick={onClose}>
      <div className="palette" onClick={e => e.stopPropagation()} onKeyDown={handleKeyDown}>
        <div className="palette-input-wrapper">
          <svg className="palette-search-icon" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>
          <input
            ref={inputRef}
            className="palette-input"
            type="text"
            placeholder="Search files, modules, and code..."
            value={query}
            onChange={e => setQuery(e.target.value)}
          />
          {hasQuery && (
            <span className="palette-meta">
              {totalModuleMatches > 0 && <span>{totalModuleMatches} modules</span>}
              {totalModuleMatches > 0 && totalFileMatches > 0 && <span className="palette-dot"> · </span>}
              {totalFileMatches > 0 && <span>{totalFileMatches} files</span>}
              {totalGrepMatches > 0 && <span className="palette-dot"> · </span>}
              {totalGrepMatches > 0 && <span>{totalGrepMatches} content matches</span>}
              {loading && <span className="palette-dot"> · </span>}
              {loading && <div className="spinner" style={{ width: 12, height: 12, display: 'inline-block' }} />}
            </span>
          )}
        </div>

        <div ref={scrollRef} className="palette-results">
          {!hasQuery && (
            <div className="palette-suggestions">
              <div className="palette-hint">
                Search file names, modules, and code contents
              </div>
            </div>
          )}

          {hasQuery && !loading && flatResults.length === 0 && (
            <div className="palette-empty">No results for "{query}"</div>
          )}

          {hasQuery && flatResults.length > 0 && (
            <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
              {virtualizer.getVirtualItems().map(vi => {
                const item = flatResults[vi.index];
                const isActive = vi.index === activeIdx;

                if (item.type === 'module-header') {
                  return (
                    <div key="mh" className="palette-section-label" style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}>
                      MODULES ({totalModuleMatches})
                    </div>
                  );
                }
                if (item.type === 'file-header') {
                  return (
                    <div key="fh" className="palette-section-label" style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}>
                      FILES ({totalFileMatches})
                    </div>
                  );
                }
                if (item.type === 'content-header') {
                  return (
                    <div key="ch" className="palette-section-label" style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}>
                      CODE SEARCH ({totalGrepMatches} in {totalGrepFiles} files)
                    </div>
                  );
                }
                if (item.type === 'module') {
                  const m = item.data;
                  return (
                    <div
                      key={`m-${m.id}`}
                      className={`palette-result palette-result-module${isActive ? ' active' : ''}`}
                      style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}
                      onClick={() => { onNavigateModule(m.id); onClose(); }}
                      onMouseEnter={() => setActiveIdx(vi.index)}
                    >
                      <svg className="palette-result-icon folder" width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M10 4H4a2 2 0 00-2 2v12a2 2 0 002 2h16a2 2 0 002-2V8a2 2 0 00-2-2h-8l-2-2z"/>
                      </svg>
                      <span className="palette-result-text">
                        <HighlightedText text={m.id} indices={m.matchedIndices} />
                      </span>
                      <span className="palette-result-badge">{m.fileCount}</span>
                    </div>
                  );
                }
                if (item.type === 'file') {
                  const f = item.data;
                  const isSelected = selected.has(f.path);
                  return (
                    <div
                      key={`f-${f.path}`}
                      className={`palette-result palette-result-file${isActive ? ' active' : ''}${isSelected ? ' selected' : ''}`}
                      style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}
                      onClick={() => onToggleFile(f.path)}
                      onMouseEnter={() => setActiveIdx(vi.index)}
                    >
                      <FileIcon ext={f.ext} size={16} />
                      <div className="palette-result-file-info">
                        <div className="palette-result-filename">
                          <HighlightedText text={f.filename} indices={f.filenameIndices} />
                          {isSelected && <span className="palette-check">✓</span>}
                        </div>
                        <div className="palette-result-path">{f.dir}</div>
                      </div>
                      <span className="palette-result-desc">{f.desc}</span>
                    </div>
                  );
                }
                if (item.type === 'grep-file') {
                  const isSelected = selected.has(item.data.path);
                  return (
                    <div
                      key={`gf-${item.data.path}`}
                      className={`palette-result grep-file-header${isActive ? ' active' : ''}${isSelected ? ' selected' : ''}`}
                      style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}
                      onClick={() => onToggleFile(item.data.path)}
                      onMouseEnter={() => setActiveIdx(vi.index)}
                    >
                      <FileIcon ext={item.data.path.slice(item.data.path.lastIndexOf('.'))} size={14} />
                      <span className="grep-file-path">{item.data.path}</span>
                      <span className="palette-result-badge">{item.data.matches.length}</span>
                      {isSelected && <span className="palette-check">✓</span>}
                    </div>
                  );
                }

                // grep-match
                return (
                  <div
                    key={`gm-${item.filePath}-${item.lineNum}`}
                    className={`palette-result grep-match-row${isActive ? ' active' : ''}`}
                    style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}
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

        {hasQuery && (results.files.length > 0 || (grepResults && grepResults.results.length > 0)) && (
          <div className="palette-global-search">
            <button className="palette-global-btn smart-select-btn" onClick={() => {
              const scoreMap = new Map<string, number>();
              for (const f of results.files) {
                scoreMap.set(f.path, Math.max(scoreMap.get(f.path) ?? 0, f.score));
              }
              if (grepResults) {
                for (const g of grepResults.results) {
                  scoreMap.set(g.path, Math.max(scoreMap.get(g.path) ?? 0, g.score));
                }
              }
              const ranked = [...scoreMap.entries()]
                .sort((a, b) => b[1] - a[1])
                .slice(0, 50);
              const paths = ranked.map(([path]) => path);
              const scores = new Map(ranked);
              onSmartSelect(paths, scores, query.trim());
              onClose();
            }}>
              Select top {(() => {
                const allPaths = new Set<string>();
                for (const f of results.files) allPaths.add(f.path);
                if (grepResults) {
                  for (const g of grepResults.results) allPaths.add(g.path);
                }
                return Math.min(allPaths.size, 50);
              })()} files
            </button>
          </div>
        )}

        <div className="palette-footer">
          <span><kbd>↑↓</kbd> navigate</span>
          <span><kbd>⏎</kbd> select</span>
          <span><kbd>esc</kbd> close</span>
        </div>
      </div>
    </div>
  );
}
