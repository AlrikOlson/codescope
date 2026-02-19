import './styles/activity-bar.css';

export type ActivityPanel = 'search' | 'tree' | 'map' | 'context' | 'stats' | 'graph';

interface Props {
  active: ActivityPanel | null;
  onSelect: (panel: ActivityPanel) => void;
  contextCount: number;
  theme: 'system' | 'light' | 'dark';
  onCycleTheme: () => void;
}

export default function ActivityBar({ active, onSelect, contextCount, theme, onCycleTheme }: Props) {
  const toggle = (panel: ActivityPanel) => {
    onSelect(panel);
  };

  return (
    <nav className="activity-bar" role="tablist" aria-orientation="vertical">
      <button
        className={`activity-bar-item${active === 'search' ? ' active' : ''}`}
        onClick={() => toggle('search')}
        title="Search (Ctrl+K)"
        role="tab"
        aria-selected={active === 'search'}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
        </svg>
      </button>

      <button
        className={`activity-bar-item${active === 'tree' ? ' active' : ''}`}
        onClick={() => toggle('tree')}
        title="Module Tree (Ctrl+1)"
        role="tab"
        aria-selected={active === 'tree'}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M3 3h7v7H3zM14 3h7v4h-7zM14 10h7v4h-7zM3 13h7v8H3zM14 17h7v4h-7z"/>
        </svg>
      </button>

      <button
        className={`activity-bar-item${active === 'map' ? ' active' : ''}`}
        onClick={() => toggle('map')}
        title="Codebase Map (Ctrl+2)"
        role="tab"
        aria-selected={active === 'map'}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <rect x="3" y="3" width="8" height="8"/><rect x="13" y="3" width="8" height="4"/>
          <rect x="13" y="9" width="8" height="5"/><rect x="3" y="13" width="8" height="8"/>
          <rect x="13" y="16" width="8" height="5"/>
        </svg>
      </button>

      <button
        className={`activity-bar-item${active === 'context' ? ' active' : ''}`}
        onClick={() => toggle('context')}
        title="Context Panel (Ctrl+3)"
        role="tab"
        aria-selected={active === 'context'}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
          <polyline points="14 2 14 8 20 8"/>
          <line x1="16" y1="13" x2="8" y2="13"/>
          <line x1="16" y1="17" x2="8" y2="17"/>
        </svg>
        {contextCount > 0 && (
          <span className="activity-bar-badge">{contextCount > 99 ? '99+' : contextCount}</span>
        )}
      </button>

      <button
        className={`activity-bar-item${active === 'stats' ? ' active' : ''}`}
        onClick={() => toggle('stats')}
        title="Stats Dashboard (Ctrl+5)"
        role="tab"
        aria-selected={active === 'stats'}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M18 20V10M12 20V4M6 20v-6"/>
        </svg>
      </button>

      <button
        className={`activity-bar-item${active === 'graph' ? ' active' : ''}`}
        onClick={() => toggle('graph')}
        title="Dependency Graph (Ctrl+6)"
        role="tab"
        aria-selected={active === 'graph'}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="6" cy="6" r="3"/><circle cx="18" cy="6" r="3"/>
          <circle cx="6" cy="18" r="3"/><circle cx="18" cy="18" r="3"/>
          <path d="M9 6h6M6 9v6M18 9v6M9 18h6"/>
        </svg>
      </button>

      <div className="activity-bar-spacer" />

      <div className="activity-bar-bottom">
        <button
          className="activity-bar-item"
          onClick={onCycleTheme}
          title={`Theme: ${theme}`}
        >
          {theme === 'dark' ? (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
          ) : theme === 'light' ? (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="5"/><path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/></svg>
          ) : (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/><path d="M16 12a4 4 0 0 1-4 4" strokeDasharray="2 2"/></svg>
          )}
        </button>
      </div>
    </nav>
  );
}
