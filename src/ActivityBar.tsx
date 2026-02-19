import './styles/activity-bar.css';

export type ActivityPanel = 'search' | 'tree';

interface Props {
  active: ActivityPanel | null;
  onSelect: (panel: ActivityPanel) => void;
  contextCount: number;
  theme: 'system' | 'light' | 'dark';
  onCycleTheme: () => void;
}

export default function ActivityBar({ active, onSelect, contextCount, theme, onCycleTheme }: Props) {
  return (
    <nav className="activity-bar" role="tablist" aria-orientation="vertical">
      <button
        className={`activity-bar-item${active === 'tree' ? ' active' : ''}`}
        onClick={() => onSelect('tree')}
        title="Explorer (Ctrl+B)"
        role="tab"
        aria-selected={active === 'tree'}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M3 3h7v7H3zM14 3h7v4h-7zM14 10h7v4h-7zM3 13h7v8H3zM14 17h7v4h-7z"/>
        </svg>
        {contextCount > 0 && (
          <span className="activity-bar-badge">{contextCount > 99 ? '99+' : contextCount}</span>
        )}
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
