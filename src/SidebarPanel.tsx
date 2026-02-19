import type { ReactNode } from 'react';
import type { ActivityPanel } from './ActivityBar';

interface Props {
  active: ActivityPanel | null;
  panels: Partial<Record<ActivityPanel, ReactNode>>;
}

export default function SidebarPanel({ active, panels }: Props) {
  if (!active) return null;

  const content = panels[active];
  if (!content) return null;

  return (
    <div className="sidebar-panel" role="tabpanel">
      {content}
    </div>
  );
}
