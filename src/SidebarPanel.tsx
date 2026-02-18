import type { ReactNode } from 'react';
import type { ActivityPanel } from './ActivityBar';

interface Props {
  active: ActivityPanel | null;
  panels: Partial<Record<ActivityPanel, ReactNode>>;
  width: number;
  onResizeStart: (e: React.MouseEvent) => void;
  isDragging: boolean;
}

export default function SidebarPanel({ active, panels, width, onResizeStart, isDragging }: Props) {
  if (!active) return null;

  const content = panels[active];
  if (!content) return null;

  return (
    <div className="sidebar-panel-wrapper" style={{ width: width + 2, display: 'flex', flexShrink: 0, overflow: 'hidden' }}>
      <div
        className="sidebar-panel"
        style={{ width, flex: '1 1 auto' }}
        role="tabpanel"
      >
        {content}
      </div>
      <div
        className={`resize-handle${isDragging ? ' dragging' : ''}`}
        onMouseDown={onResizeStart}
      />
    </div>
  );
}
