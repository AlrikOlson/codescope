import { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import TreeSidebar from './TreeSidebar';
import FileList from './FileList';
import CodebaseMap from './treemap/CodebaseMap';
import DependencyGraph from './depgraph/DependencyGraph';
import SearchSidebar from './SearchSidebar';
import FilePreview from './FilePreview';
import StatsDashboard from './StatsDashboard';
import ActivityBar from './ActivityBar';
import type { ActivityPanel } from './ActivityBar';
import SidebarPanel from './SidebarPanel';
import { useDataFetch } from './hooks/useDataFetch';
import { toggleModule, selectWithDeps, resolveCategoryPath } from './selectionActions';
import './styles/layout.css';

type ViewMode = 'list' | 'map' | 'graph' | 'stats';

const VIEW_LABELS: Record<ViewMode, string> = {
  list: 'Files',
  map: 'Map',
  graph: 'Graph',
  stats: 'Stats',
};

export default function App() {
  const { tree, manifest, deps, loading } = useDataFetch();
  const appRef = useRef<HTMLDivElement>(null);

  // Navigation state
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [activeCategory, setActiveCategory] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [globalSearch, setGlobalSearch] = useState<string | null>(null);
  const [globalSearchPaths, setGlobalSearchPaths] = useState<Map<string, number> | null>(null);

  // View state
  const [viewMode, setViewMode] = useState<ViewMode>('list');
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const [activePanel, setActivePanel] = useState<ActivityPanel | null>('tree');

  // Theme
  const [theme, setTheme] = useState<'system' | 'light' | 'dark'>(() => {
    return (localStorage.getItem('codescope-theme') as 'system' | 'light' | 'dark') || 'system';
  });

  useEffect(() => {
    const root = document.documentElement;
    localStorage.setItem('codescope-theme', theme);
    if (theme === 'system') {
      root.style.removeProperty('color-scheme');
      root.removeAttribute('data-theme');
    } else {
      root.style.colorScheme = theme;
      root.setAttribute('data-theme', theme);
    }
  }, [theme]);

  const cycleTheme = useCallback(() => {
    setTheme(t => t === 'dark' ? 'light' : t === 'light' ? 'system' : 'dark');
  }, []);

  // Activity bar panel selection — just toggles sidebar, no view mode change
  const handlePanelSelect = useCallback((panel: ActivityPanel) => {
    setActivePanel(prev => prev === panel ? null : panel);
  }, []);

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;

      if (mod && e.key === 'k') {
        e.preventDefault();
        setActivePanel('search');
      }
      if (mod && e.key === 'b') {
        e.preventDefault();
        setActivePanel(prev => prev ? null : 'tree');
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  // Mouse-reactive ambient glow
  useEffect(() => {
    const el = appRef.current;
    if (!el) return;
    let raf = 0;
    const onMove = (e: MouseEvent) => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        el.style.setProperty('--mx', `${e.clientX}px`);
        el.style.setProperty('--my', `${e.clientY}px`);
      });
    };
    window.addEventListener('mousemove', onMove, { passive: true });
    return () => {
      window.removeEventListener('mousemove', onMove);
      cancelAnimationFrame(raf);
    };
  }, []);

  // Tree navigation handlers
  const handleToggle = useCallback((id: string) => {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  }, []);

  const handleSelect = useCallback((id: string) => {
    setActiveCategory(id);
    setGlobalSearch(null);
  }, []);

  // Search results callback from SearchSidebar
  const handleSearchResults = useCallback((paths: Map<string, number>, query: string) => {
    setGlobalSearch(query);
    setGlobalSearchPaths(paths);
    setActiveCategory(null);
  }, []);

  const handleNavigateModule = useCallback((id: string) => {
    const parts = id.split(' > ');
    setExpanded(prev => {
      const next = new Set(prev);
      let path = '';
      for (const part of parts) {
        path = path ? `${path} > ${part}` : part;
        next.add(path);
      }
      return next;
    });
    setActiveCategory(id);
    setActivePanel('tree');
    setViewMode('list');
  }, []);

  // File selection handlers
  const handleToggleFile = useCallback((path: string) => {
    setSelected(prev => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path); else next.add(path);
      return next;
    });
  }, []);

  const handleSelectAll = useCallback((paths: string[]) => {
    setSelected(prev => {
      const next = new Set(prev);
      paths.forEach(p => next.add(p));
      return next;
    });
  }, []);

  const handleDeselectAll = useCallback((paths: string[]) => {
    setSelected(prev => {
      const next = new Set(prev);
      paths.forEach(p => next.delete(p));
      return next;
    });
  }, []);

  // Sidebar checkboxes — toggle one module's files
  const handleToggleModule = useCallback((moduleId: string) => {
    setSelected(prev => toggleModule(moduleId, manifest, prev));
  }, [manifest]);

  // Graph/treemap clicks — select module + deps, expand tree, highlight, scroll
  const handleSelectModule = useCallback((id: string) => {
    if (!deps) return;
    setSelected(prev => selectWithDeps(id, deps, manifest, prev));
    const catPath = resolveCategoryPath(id, deps);
    if (catPath) {
      const parts = catPath.split(' > ');
      setExpanded(prev => {
        const next = new Set(prev);
        let path = '';
        for (const part of parts) {
          path = path ? `${path} > ${part}` : part;
          next.add(path);
        }
        return next;
      });
      setActiveCategory(catPath);
    }
    setActivePanel('tree');
  }, [deps, manifest]);

  const handleClear = useCallback(() => setSelected(new Set()), []);
  const handleCollapseAll = useCallback(() => setExpanded(new Set()), []);

  const handleExpandAll = useCallback(() => {
    if (!tree) return;
    const topLevel = new Set<string>();
    for (const key of Object.keys(tree)) {
      if (key !== '_files') topLevel.add(key);
    }
    setExpanded(topLevel);
  }, [tree]);

  const handlePreview = useCallback((path: string) => {
    setPreviewPath(prev => prev === path ? null : path);
  }, []);


  // Breadcrumb segments from activeCategory
  const breadcrumbs = useMemo(() => {
    if (!activeCategory) return [];
    const parts = activeCategory.split(' > ');
    return parts.map((name, i) => ({
      name,
      id: parts.slice(0, i + 1).join(' > '),
    }));
  }, [activeCategory]);

  if (loading) {
    return (
      <div className="loading-screen">
        <div className="loading-logo">CodeScope</div>
        <div className="loading-bar" />
        <div className="loading-text">Scanning codebase...</div>
      </div>
    );
  }

  return (
    <div className="app" ref={appRef}>
      <div className="app-ambient" />
      <div className="app-glow" />
      <div className="app-grid" />
      <div className="app-sweep" />
      <div className="app-noise" />
      <div className="app-vignette" />

      <header className="topbar chromatic-always">
        <h1 className="logo">CS</h1>
        {breadcrumbs.length > 0 && (
          <nav className="breadcrumb">
            {breadcrumbs.map((bc, i) => (
              <span key={bc.id}>
                {i > 0 && <span className="breadcrumb-sep">/</span>}
                <button
                  className={`breadcrumb-item${i === breadcrumbs.length - 1 ? ' current' : ''}`}
                  onClick={() => handleSelect(bc.id)}
                >
                  {bc.name}
                </button>
              </span>
            ))}
          </nav>
        )}
        <div className="view-switcher">
          {(['list', 'map', 'graph', 'stats'] as const).map(mode => (
            <button
              key={mode}
              className={`view-switcher-btn${viewMode === mode ? ' active' : ''}`}
              onClick={() => setViewMode(mode)}
            >
              {VIEW_LABELS[mode]}
            </button>
          ))}
        </div>
        <button className="search-trigger" onClick={() => setActivePanel('search')}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>
          <span>Search...</span>
          <kbd>Ctrl+K</kbd>
        </button>
        <div className="topbar-spacer" />
      </header>

      <div className="layout">
        <ActivityBar
          active={activePanel}
          onSelect={handlePanelSelect}
          contextCount={selected.size}
          theme={theme}
          onCycleTheme={cycleTheme}
        />

        <SidebarPanel
          active={activePanel}
          panels={{
            search: (
              <SearchSidebar
                selected={selected}
                onToggleFile={handleToggleFile}
                onPreview={handlePreview}
                onSearchResults={handleSearchResults}
                autoFocus={activePanel === 'search'}
              />
            ),
            tree: (
              <TreeSidebar
                tree={tree}
                manifest={manifest}
                expanded={expanded}
                activeCategory={activeCategory}
                selected={selected}
                globalSearch={globalSearch}
                onToggle={handleToggle}
                onSelect={handleSelect}
                onToggleFile={handleToggleFile}
                onToggleModule={handleToggleModule}
                onClear={handleClear}
                onCollapseAll={handleCollapseAll}
                onExpandAll={handleExpandAll}
              />
            ),
          }}
        />

        <div className="main-content">
          {viewMode === 'list' && (
            <div className="list-content">
              <FileList
                manifest={manifest}
                deps={deps}
                activeCategory={activeCategory}
                globalSearch={globalSearch}
                globalSearchPaths={globalSearchPaths}
                selected={selected}
                onToggleFile={handleToggleFile}
                onSelectAll={handleSelectAll}
                onDeselectAll={handleDeselectAll}
                onPreview={handlePreview}
              />
              {previewPath && (
                <div style={{ width: 400, flexShrink: 0, borderLeft: '1px solid var(--border)' }}>
                  <FilePreview
                    path={previewPath}
                    manifest={manifest}
                    selected={selected}
                    onClose={() => setPreviewPath(null)}
                    onToggleFile={handleToggleFile}
                    isMaximized={false}
                    onToggleMaximize={() => {}}
                  />
                </div>
              )}
            </div>
          )}
          {viewMode === 'map' && (
            <CodebaseMap
              tree={tree}
              manifest={manifest}
              activeCategory={activeCategory}
              selected={selected}
              onNavigateModule={handleNavigateModule}
              onToggleFile={handleToggleFile}
              onSelectModule={handleSelectModule}
            />
          )}
          {viewMode === 'graph' && deps && (
            <DependencyGraph
              deps={deps}
              activeCategory={activeCategory}
              searchTerm=""
              globalSearch={globalSearch}
              selected={selected}
              manifest={manifest}
              onNavigateModule={handleNavigateModule}
              onSelectModule={handleSelectModule}
            />
          )}
          {viewMode === 'stats' && (
            <StatsDashboard
              manifest={manifest}
              tree={tree}
            />
          )}
        </div>
      </div>
    </div>
  );
}
