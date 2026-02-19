import { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import TreeSidebar from './TreeSidebar';
import FileList from './FileList';
import CodebaseMap from './treemap/CodebaseMap';
import DependencyGraph from './depgraph/DependencyGraph';
import ContextPanel from './ContextPanel';
import SearchSidebar from './SearchSidebar';
import FilePreview from './FilePreview';
import StatsDashboard from './StatsDashboard';
import ActivityBar from './ActivityBar';
import type { ActivityPanel } from './ActivityBar';
import SidebarPanel from './SidebarPanel';
import { useDataFetch } from './hooks/useDataFetch';
import { useSidebarResize } from './hooks/useSidebarResize';
import { usePreviewResize } from './hooks/usePreviewResize';
import { useBreakpoint } from './hooks/useBreakpoint';
import './styles/layout.css';

export default function App() {
  const { tree, manifest, deps, loading } = useDataFetch();
  const { sidebarWidth, startResize, isDragging } = useSidebarResize(300);
  const { previewWidth, startResize: startPreviewResize, isDragging: isPreviewDragging } = usePreviewResize(480);
  const appRef = useRef<HTMLDivElement>(null);
  const breakpoint = useBreakpoint();

  // Navigation state
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [activeCategory, setActiveCategory] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [globalSearch, setGlobalSearch] = useState<string | null>(null);
  const [globalSearchPaths, setGlobalSearchPaths] = useState<Map<string, number> | null>(null);

  // View state
  const [viewMode, setViewMode] = useState<'list' | 'map' | 'graph' | 'stats'>('list');
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

  const handleViewChange = useCallback((mode: typeof viewMode) => {
    if ((document as any).startViewTransition) {
      (document as any).startViewTransition(() => setViewMode(mode));
    } else {
      setViewMode(mode);
    }
  }, []);

  // Activity bar panel selection — also switches view mode for stats/graph/map
  const handlePanelSelect = useCallback((panel: ActivityPanel) => {
    if (panel === 'stats') {
      handleViewChange('stats');
      setActivePanel('stats');
    } else if (panel === 'graph') {
      handleViewChange('graph');
      setActivePanel('graph');
    } else if (panel === 'map') {
      handleViewChange('map');
      setActivePanel('map');
    } else {
      // Switch to list view when selecting search/tree/context
      if (viewMode === 'stats' || viewMode === 'graph' || viewMode === 'map') {
        handleViewChange('list');
      }
      setActivePanel(prev => prev === panel ? null : panel);
    }
  }, [handleViewChange, viewMode]);

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
      if (mod && e.key >= '1' && e.key <= '6') {
        e.preventDefault();
        const panels: ActivityPanel[] = ['search', 'tree', 'map', 'context', 'stats', 'graph'];
        handlePanelSelect(panels[parseInt(e.key) - 1]);
      }
      if (e.key === 'Escape' && breakpoint === 'compact' && activePanel) {
        setActivePanel(null);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [breakpoint, activePanel, handlePanelSelect]);

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
    // Switch to tree view to show the navigation
    if (activePanel === 'search' || activePanel === 'map') {
      setActivePanel('tree');
    }
    if (viewMode !== 'list') {
      handleViewChange('list');
    }
  }, [activePanel, viewMode, handleViewChange]);

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

  const handleRemoveFile = useCallback((path: string) => {
    setSelected(prev => {
      const next = new Set(prev);
      next.delete(path);
      return next;
    });
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

  // Determine if context panel should be pinned (wide screens, only when preview is closed)
  const contextPinned = breakpoint === 'wide' && selected.size > 0 && !previewPath;

  // Layout CSS classes
  const layoutClasses = [
    'layout',
    !activePanel ? 'sidebar-hidden' : '',
    contextPinned ? 'context-pinned' : '',
  ].filter(Boolean).join(' ');

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
        <button className="search-trigger" onClick={() => setActivePanel('search')}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>
          <span>Search...</span>
          <kbd>Ctrl+K</kbd>
        </button>
        <div className="topbar-spacer" />
      </header>

      {/* Compact mode backdrop — outside grid to avoid taking a grid cell */}
      {breakpoint === 'compact' && activePanel && (
        <div
          className={`sidebar-backdrop${activePanel ? ' visible' : ''}`}
          onClick={() => setActivePanel(null)}
        />
      )}

      <div className={layoutClasses} style={{ '--sidebar-w': activePanel ? `${sidebarWidth + 2}px` : '0px' } as React.CSSProperties}>
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
                onToggle={handleToggle}
                onSelect={handleSelect}
                onToggleFile={handleToggleFile}
                onCollapseAll={handleCollapseAll}
                onExpandAll={handleExpandAll}
              />
            ),
            context: !contextPinned ? (
              <ContextPanel
                selected={selected}
                manifest={manifest}
                searchQuery={globalSearch}
                onClear={handleClear}
                onRemoveFile={handleRemoveFile}
              />
            ) : undefined,
          }}
          width={sidebarWidth}
          onResizeStart={startResize}
          isDragging={isDragging}
        />

        <div className="main-content">
          {(viewMode === 'list' || (viewMode !== 'map' && viewMode !== 'graph' && viewMode !== 'stats')) && (
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
                <>
                  <div
                    className={`preview-resize-handle${isPreviewDragging ? ' dragging' : ''}`}
                    onMouseDown={startPreviewResize}
                  />
                  <div style={{ width: previewWidth, flexShrink: 0 }}>
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
                </>
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
            />
          )}
          {viewMode === 'stats' && (
            <StatsDashboard
              manifest={manifest}
              tree={tree}
            />
          )}
        </div>

        {/* Pinned context panel on wide screens */}
        {contextPinned && (
          <div className="context-right-sidebar">
            <ContextPanel
              selected={selected}
              manifest={manifest}
              searchQuery={globalSearch}
              onClear={handleClear}
              onRemoveFile={handleRemoveFile}
            />
          </div>
        )}
      </div>
    </div>
  );
}
